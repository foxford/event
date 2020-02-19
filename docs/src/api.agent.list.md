# agent.list

List active [agents](api.agent.md#agent) in a [room](api.room#room).

The _room_ must be opened.

## Authorization

The current _agent_ must be [entered](api.room.enter.md) to the _room_.

## Parameters

Name    | Type   | Default    | Description
------- | ------ | ---------- | --------------------
room_id | string | _required_ | The room identifier.
offset  | int    | _optional_ | Pagination offset.
limit   | int    |         25 | Pagination limit.

## Response

**Status:** 200.

**Payload:** list of [agents](api.agent.md#agent).
