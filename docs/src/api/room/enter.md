# room.enter

Enter a [room](../room.md#room).

The room must be opened.

On entering the current _agent_ starts to receive room notifications including real-time
[events](../event.md#event) and can call endpoints that require _room_ entering.

Entered [agents](../agent.md#agent) appear in the active _agents_ [list](../agent/list.md).

For implementation details see the [room entrance](../../impl/room_entrance.md).

## Authorization

The tenant authorizes the current _agent_ for `subscribe` action on
`["rooms", room_id, "events"]` object.

## Multicast request

Name | Type | Default    | Description
---- | ---- | ---------- | --------------------
id   | uuid | _required_ | The room identifier.

## Unicast response

**Status:** 202.

**Payload:** empty object.

Receiving the response means that entering is still in progress and the agent is not in the room yet. Before making any requests that require room access one must wait
for the `room.enter` broadcast notification that confirms the entrance. The description is below.

## Broadcast event

A notification is being sent to all [agents](../agent.md#agent) that [are in](../room/enter.md) the room.

**URI:** `rooms/:room_id/events`

**Label:** `room.enter`

**Payload:**

Name     | Type     | Default    | Description
-------- | -------- | ---------- | --------------------
id       | uuid     | _required_ | The room identifier.
agent_id | agent_id | _required_ | The agent identifier.
