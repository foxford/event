//! The basic idea of room adjustment is cloning the room and shifting events' timestamps
//! according to editings.
//!
//! There are editings of two types:
//!
//! 1. Video segments obtained from conference. The video recording is being concatenated
//!    of several segments. There are time gaps between recording these segments but in the
//!    video there are no gaps. So to keep events in sync with video we need to shift events
//!    based on the list of start-stop relative timestamps.
//! 2. Cut-start/stop events that atr being created by a moderator during the translation.
//!    This is live editing: the moderator lets us know that (s)he wants to exclude particular
//!    segments of the translation from the recording.
//!
//! We'd like to allow the moderator to change cut-start/stop events with the media editor.
//!
//! So we would like to have three rooms:
//! 1. Live – initial events recorded during the translation.
//! 2. Original – clone of the live room with video segments from conference applied but not
//!    cut-start/stop events.
//! 3. Modified – the clone of the original room with cut-start/stop events applied. This room
//!    is the room that is being showed to a user in on-demand mode and also this room can be
//!    recreated from the original one with new cut-start/stop events from the media editor.
//!
//! The backend already can apply editings but the problem is that it applies both kinds at once.
//! This algorithm works around this problem by removing cut-start/stop events from the live room
//! like they have never been there to avoid their application when creating the original room.
//!
//! So the basic algorithm is the following:
//! 1. Close the live room and set video segments.
//! 2. Clone the live room into the modified room with both video segments & cut-start/stop
//!    events applied.
//! 3. Remove cut-start/stop events from the live room.
//! 4. Clone the live room into the original room with video segments applied only.
//!
//! Rooms are being synchronized in the backend and in the local DB so in total there are 6 rooms.

use chrono::{DateTime, Utc};
use failure::{format_err, Error};
use log::info;
use svc_authn::Authenticable;

use crate::backend::types::{Room as BackendRoom, RoomMetadata as BackendRoomMetadata};
use crate::backend::Client as BackendClient;
use crate::db::adjustment::{InsertQuery as AdjustmentInsertQuery, Segment};
use crate::db::room::{InsertQuery as RoomInsertQuery, Object as LocalRoom};
use crate::db::ConnectionPool as Db;

pub(crate) async fn call(
    db: Db,
    backend: &BackendClient,
    account: &impl Authenticable,
    local_live_room: &LocalRoom,
    started_at: DateTime<Utc>,
    segments: Vec<Segment>,
    offset: i64,
) -> Result<(LocalRoom, LocalRoom, Vec<Segment>), Error> {
    info!(
        "Room adjustment task started for room_id = '{}'",
        local_live_room.id()
    );
    let start_timestamp = Utc::now();

    // Set local live room metadata.
    {
        let conn = db.get()?;

        AdjustmentInsertQuery::new(local_live_room.id(), started_at, segments.clone(), offset)
            .execute(&conn)?;
    }

    // Close backend live room.
    backend
        .close_room(account, &local_live_room.audience(), local_live_room.id())
        .await?;

    // Set backend live room metadata.
    backend
        .set_room_metadata(
            account,
            &local_live_room.audience(),
            local_live_room.id(),
            &BackendRoomMetadata {
                started_at,
                time: segments,
                preroll: offset,
            },
        )
        .await?;

    // Clone the backend live room and offset according to both segments and cut-start/stop events.
    let (backend_modified_room, modified_segments) = backend
        .transcode_stream(account, &local_live_room.audience(), local_live_room.id())
        .await?;

    // Сreate local modified room.
    let local_modified_room =
        create_local_room(db.clone(), &local_live_room, &backend_modified_room)?;

    // Get cut-start/stop events from backend live room.
    let cut_events = backend
        .get_events(
            account,
            &local_live_room.audience(),
            local_live_room.id(),
            "stream",
        )
        .await?;

    // Delete these events from the backend live room.
    for event in &cut_events {
        backend
            .delete_event(
                account,
                &local_live_room.audience(),
                local_live_room.id(),
                "stream",
                event.id,
                Some("adjustment"),
            )
            .await?;
    }

    // Clone the backend live room and offset according to segments only.
    let (backend_original_room, _original_segments) = backend
        .transcode_stream(account, &local_live_room.audience(), local_live_room.id())
        .await?;

    // Create local original room.
    let local_original_room = create_local_room(db, &local_live_room, &backend_original_room)?;

    info!(
        "Room adjustment task successfully finished for room_id = '{}', duration = {} ms",
        local_live_room.id(),
        (Utc::now() - start_timestamp).num_milliseconds()
    );

    Ok((local_original_room, local_modified_room, modified_segments))
}

fn create_local_room(
    db: Db,
    local_source_room: &LocalRoom,
    backend_room: &BackendRoom,
) -> Result<LocalRoom, Error> {
    let mut query = RoomInsertQuery::new(
        backend_room.id,
        &local_source_room.audience(),
        local_source_room.time().to_owned(),
    );

    if let Some(tags) = local_source_room.tags() {
        query = query.tags(tags.to_owned());
    }

    query = query.source_room_id(local_source_room.id());

    let conn = db.get()?;

    query
        .execute(&conn)
        .map_err(|err| format_err!("Failed to insert room: {}", err))
}
