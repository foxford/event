# room.whiteboard_access

Used to update whiteboard access in a room (if the room has `validate_whiteboard_access` flag set).

## Authorization

The tenant authorizes the current _agent_ for `update` action on `["rooms", room_id]` object.

## Multicast request

Name                | Type                  | Default    | Description
------------------- | ----                  | ---------- | --------------------
id                  | uuid                  | _required_ | The room identifier.
whiteboard_access   | {account_id: bool}    | _required_ | Map of the events types to lock from creation. Works like diff - will be merged into current whiteboard access

## Unicast response

**Status:** 200.

**Payload:** [room](../room.md#room) object.

## Broadcast event

A notification is being sent to the _audience_ topic.

**URI:** `audiences/:audience/events`

**Label:** `room.update`.
**Payload:** [room](../room.md#room) object.
