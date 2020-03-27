# room.update

Update a [room](../room.md#room).

The room must not be closed. Opened and not yet opened rooms are fine.

Opening time must not be in the past.

Opening time can't be changed if the room is already opened.

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

A notification is being sent to the _room_ topic.

**URI:** `rooms/:room_id/events`

**Label:** `room.update`.

**Payload:** [room](../room.md#room) object.
