# change.delete

Delete an [change](../change.md#change).

## Authorization

The tenant authorizes the current _agent_ for `update` action on `["classrooms", classroom_id]` object.

## Multicast request

Name  | Type       | Default    | Description
----- | ---------- | ---------- | ------------------------------------------------------------
id    | uuid       | _required_ | Change id

## Unicast response

**Status:** 200.

**Payload:** deleted [change](../change.md#change) object.
