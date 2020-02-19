# Agent

An _agent_ is an entity for tracking online presence of MQTT agents in [rooms](room.md#room).

If an _agent_ has [entered](room/enter.md) a _room_ it will be [listed](agent/list.md)
in the online agents list and get notified about [events](event.md#event) that happend
in the _room_.

If an online _agent_ has [left](room/leave.md) the _room_ or disconnected from the broker
it will no longer be listed nor receive notifications.

One _agent_ may potentially enter many _rooms_.

## Properties

Name       | Type     | Default    | Description
---------- | -------- | ---------- | ----------------------------------------------------
agent_id   | agent_id | _required_ | The identifier of an agent who has entered the room.
room_id    | uuid     | _required_ | The room identifier to where the agent has entered.
created_at | int      | _required_ | Entrance timestamp in seconds.
