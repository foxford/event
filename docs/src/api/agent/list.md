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

**Payload:** list of [agents](../agent.md#agent) with `banned` property

## Properties

Name       | Type     | Default    | Description
---------- | -------- | ---------- | ----------------------------------------------------
agent_id   | agent_id | _required_ | The agent's identifier who has entered the room.
room_id    | uuid     | _required_ | The room's identifier where the agent has entered.
created_at | int      | _required_ | Entrance's timestamp in seconds.
banned     | bool     | _optional_ | Whether the agent is banned in room or not

