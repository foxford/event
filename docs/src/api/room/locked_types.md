# room.locked_types

Its possible to restrict events creation only to those allowed to update room.

## Authorization

The tenant authorizes the current _agent_ for `update` action on `["rooms", room_id]` object.

## Multicast request

Name            | Type              | Default    | Description
--------------- | ----              | ---------- | --------------------
id              | uuid              | _required_ | The room identifier.
locked_types    | {string: bool}    | _required_ | Map of the events types to lock from creation. Works like diff - will be merged into current locked types

## Unicast response

**Status:** 200.

**Payload:** [room](../room.md#room) object.

## Broadcast event

A notification is being sent to the _audience_ topic.

**URI:** `audiences/:audience/events`

**Label:** `room.update`.
**Payload:** [room](../room.md#room) object.

## Room events

Will create an event of type = `chat_lock` with data: `{"value": bool}` depending on whether type `message` was locked or not.
