# change.create

Create an [change](../change.md#change).

## Authorization

The tenant authorizes the current _agent_ for `update` action on `["rooms", room_id]` object.

## Multicast request

Name        | Type     | Default    | Description
----------- | -------- | ---------- | ------------------------------------------------------------
edition_id  | uuid     | _required_ | The edition this change belongs to.
type        | string   | _required_ | Change type, `addition`, `modification` or `removal`
event       | json     | _required_ | Object containing `event_id` and/or event related parameters (`type`, `set`, `label`, `data`, `occurred_at`, `created_by`)

## Unicast response

**Status:** 201.

**Payload:** [change](../change.md#change) object.
