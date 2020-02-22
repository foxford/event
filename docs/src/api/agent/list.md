# agent.list

List active [agents](../agent.md#agent) in a [room](../room.md#room).

The _room_ must be opened.

## Authorization

The current _agent_ must be [in](../room/enter.md) the _room_.

## Multicast request

Name    | Type   | Default    | Description
------- | ------ | ---------- | --------------------
room_id | string | _required_ | The room's identifier.
offset  | int    | _optional_ | Pagination offset.
limit   | int    |         25 | Pagination limit.

## Unicast response

**Status:** 200.

**Payload:** list of [agents](../agent.md#agent).
