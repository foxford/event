# Integration

This part describes:
- Event service's interactions across the system,
- Event's [API](api.md).

This part describes how the event service interacts in the context of the system and the overall flow
of using its [API](api.md).

## Preparing

At first, together with the creation of translation, the tenant [creates](api/room/create.md)
a _real-time_ [room](api/room.md#room).

## Real-time

When the _room's_ opening time comes, the _room_ becomes open, and _agents_
- may [enter](api/room/enter.md) the _room_,
- [get](api/state/read.md) actual [state](api/state.md#state),
- [create](api/event/create.md) [events](api/event.md#event).

As _events_ get created, other _agents_ get notified about it in real-time.

## On-demand

After the translation is done and uploaded, the tenant starts to receive its `started_at` and
`segments` from conference service at the `room.upload` notification. It adds preroll duration
as `offset` and calls _room_ [adjustment](api/room/adjust.md).

When finished, the _adjustment_ task [notifies](api/room/adjust.md#notification) the tenant passing
`original_room_id`, `modified_room_id` and `modified_segments_id`.

The `modified_room_id` is the edited _room_ according to the _segments_ and _stream editing events_ and
going to be shown for the user in on-demand mode. The frontend [lists](api/event/list.md)
events from this room when playing the record.

If `modified_segments` are different from initial _segments_, this means that _stream editing events_
impacted the stream, and the tenant must schedule retranscoding of the video with _modified segments_.

The `original_room_id` going to be passed to the media editor so a moderator could change
_stream editing events_ on post-production and create a different _modified room_.
