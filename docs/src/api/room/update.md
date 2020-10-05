# room.update

Update a [room](../room.md#room).

The room must not be closed if `time` parameter specified. Opened and not yet opened rooms are fine.

Opening time can't be changed if the room is already opened.
Closing time can be changed if the room is not yet closed (submitted opening time will be silently ignored if the room is already opened).
If closing time is in the past the room will be updated with current moment as closing time.

## Authorization

The tenant authorizes the current _agent_ for `update` action on `["rooms", room_id]` object.

## Multicast request

Name | Type       | Default    | Description
-----| ---------- | ---------- | ------------------------------------------------------------
id   | uuid       | _required_ | The room identifier.
time | [int, int] | _optional_ | A [lt, rt) range of unix time (seconds) or null (unbounded).
tags | json       | _optional_ | Tenant-specific JSON object associated with the room.

## Unicast response

**Status:** 200.

**Payload:** [room](../room.md#room) object.

## Broadcast event

A notification is being sent to the _audience_ topic.

**URI:** `audiences/:audience/events`

**Label:** `room.update`.

**Payload:** [room](../room.md#properties) object.

If the room closure date had been in the future but was moved by the update into the past, a notification will be sent to the _room_ topic.
Clients should not rely on this notification being unique.
That is this notification can reoccur even if it was sent before.

**URI:** `rooms/:room_id/events`

**Label:** `room.close`.

**Payload:** [room](../room.md#properties) object.
