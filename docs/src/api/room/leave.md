# room.leave

Leave a room [room](../room.md#room).

After leaving a room the current _agent_ stops to receive room notifications and is not able
to call endpoints that require _room_ entrance.

Left [agents](../agent.md#agent) disappear from the active _agents_ [list](../agent/list.md).

## Authorization

The current _agent_ must be [entered](../room/enter.md) to the _room_.

## Parameters

Name | Type | Default    | Description
---- | ---- | ---------- | --------------------
id   | uuid | _required_ | The room identifier.

## Response

**Status:** 202.

**Payload:** empty object.

Receiving the response doesn't yet mean that the agent has already left the room but that the
process has been initiated.

## Notification

A notification is being sent to all [agents](../agent.md#agent) that are still in
[entered](../room/enter.md) the room.
The current agent will not receive it since it's already left.

**URI:** `rooms/:room_id/events`

**Label:** `room.leave`

**Payload:**

Name     | Type     | Default    | Description
-------- | -------- | ---------- | --------------------
id       | uuid     | _required_ | The room identifier.
agent_id | agent_id | _required_ | The agent identifier.
