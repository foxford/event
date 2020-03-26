# room.read

Retrieve the [room](../room.md#room) object.

## Authorization

The tenant authorizes the current _agent_ for `read` action on `["rooms", room_id]` object.

## Multicast request

Name  | Type | Default    | Description
----- | ---- | ---------- | --------------------
id    | uuid | _required_ | The room identifier.

## Unicast response

**Status:** 200.

**Payload:** [room](../room.md#room) object.
