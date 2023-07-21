use crate::db::event::Object as Event;
use anyhow::Result;
use chrono::Duration;
use std::ops::Bound;
use std::str::FromStr;
use std::time::Duration as StdDuration;
use svc_agent::AgentId;

const NS_IN_MS: i64 = 1_000_000;

/// Turns `segments` into gaps.
pub fn invert_segments(
    segments: &[(i64, i64)],
    room_duration: Duration,
    min_segment_length: StdDuration,
) -> Result<Vec<(i64, i64)>> {
    if segments.is_empty() {
        let total_nanos = room_duration.num_nanoseconds().unwrap_or(std::i64::MAX);
        return Ok(vec![(0, total_nanos)]);
    }

    let mut gaps = Vec::with_capacity(segments.len() + 2);

    // A possible gap before the first segment.
    if let Some((first_segment_start, _)) = segments.first() {
        if *first_segment_start > 0 {
            gaps.push((0, *first_segment_start));
        }
    }

    // Gaps between segments.
    for ((_, segment_stop), (next_segment_start, _)) in segments.iter().zip(&segments[1..]) {
        gaps.push((*segment_stop, *next_segment_start));
    }

    // A possible gap after the last segment.
    if let Some((_, last_segment_stop)) = segments.last() {
        let room_duration_nanos = room_duration.num_nanoseconds().unwrap_or(std::i64::MAX);

        // Don't create segments less than `min_segment_length`
        if *last_segment_stop < room_duration_nanos
            && StdDuration::from_nanos((room_duration_nanos - last_segment_stop) as u64)
                .gt(&min_segment_length)
        {
            gaps.push((*last_segment_stop, room_duration_nanos));
        }
    }

    Ok(gaps)
}

pub fn collect_pin_segments(
    pin_events: &[Event],
    event_room_offset: Duration,
    recording_created_by: &AgentId,
    recording_end: i64,
) -> Vec<(Bound<i64>, Bound<i64>)> {
    let mut pin_segments = vec![];
    let mut pin_start = None;

    let mut add_segment = |start, end| {
        if start <= end && start >= 0 && end <= recording_end {
            pin_segments.push((Bound::Included(start), Bound::Excluded(end)));
        }
    };

    for event in pin_events {
        if let Some(agent_id) = event
            .data()
            .get("agent_id")
            .and_then(|v| v.as_str())
            .and_then(|v| AgentId::from_str(v).ok())
        {
            // Shift from the event room's dimension to the recording's dimension.
            let occurred_at = event.occurred_at() / NS_IN_MS - event_room_offset.num_milliseconds();

            if agent_id == *recording_created_by {
                // Stream was pinned.
                // Its possible that teacher pins someone twice in a row
                // Do nothing in that case
                if pin_start.is_none() {
                    pin_start = Some(occurred_at);
                }
            } else if let Some(pinned_at) = pin_start {
                // stream was unpinned
                // its possible that pinned_at equals unpin's occurred_at after adjust
                // we skip segments like that
                if occurred_at > pinned_at {
                    add_segment(pinned_at, occurred_at);
                }
                pin_start = None;
            }
        }
    }

    // If the stream hasn't got unpinned since some moment then add a pin segment to the end
    // of the recording to keep it pinned.
    if let Some(start) = pin_start {
        add_segment(start, recording_end);
    }

    pin_segments
}
