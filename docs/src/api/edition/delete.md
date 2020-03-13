# edition.delete

Delete an [edition](../edition.md#edition).

## Authorization

The tenant authorizes the current _agent_ for `update` action on `["rooms", room_id]` object.

## Multicast request

Name  | Type       | Default    | Description
----- | ---------- | ---------- | ------------------------------------------------------------
id    | uuid       | _required_ | Edition id

## Unicast response

**Status:** 200.

**Payload:** deleted [edition](../edition.md#edition) object.
