# edition.create

Create an [edition](../edition.md#edition).

## Authorization

The tenant authorizes the current _agent_ for `update` action on `["rooms", room_id]` object.

## Multicast request

Name     | Type       | Default    | Description
-------- | ---------- | ---------- | ------------------------------------------------------------
room_id  | uuid       | _required_ | The room audience.

## Unicast response

**Status:** 201.

**Payload:** [edition](../edition.md#edition) object.