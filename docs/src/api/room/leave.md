# room.leave

Leave a room [room](../room.md#room).

After leaving a room, the current _agent_ stops to receive room notifications and can not call endpoints that require _room_ entering.

Absent [agents](../agent.md#agent) disappear from the active _agents_ [list](../agent/list.md).

## Authorization

The current _agent_ must [be in](../room/enter.md) the _room_.

## Multicast request

Name | Type | Default    | Description
---- | ---- | ---------- | --------------------
id   | uuid | _required_ | The room identifier.

## Unicast response

**Status:** 202.

**Payload:** empty object.

Receiving the response means that leaving still in progress and the agent is not out of the room yet.

## Broadcast event

A notification is being sent to all [agents](../agent.md#agent) that are [still in](../room/enter.md) the room.
The current agent stays uninformed.

**URI:** `rooms/:room_id/events`

**Label:** `room.leave`

**Payload:**

Name     | Type     | Default    | Description
-------- | -------- | ---------- | --------------------
id       | uuid     | _required_ | The room identifier.
agent_id | agent_id | _required_ | The agent identifier.
