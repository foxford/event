# Room

A room is scope for storing [events](event.md#event) sharing them between
[agents](agent.md#agent).

A room belongs to _audience_ that is a scope of a tenant. So tenants controls an _audience_ that
contains many rooms. The tenant also may associate some aribitraty _tags_ to the room to keep the
linkage with its internal entities.

A room has a _time_ period when its open i.e. available for creating events.

A room may be derived from another room during the process of [adjustment](room/adjust.md).
The room from which the current room is derived is being called the _source room_.

## Properties

Name           | Type       | Default    | Description
---------------| ---------- | ---------- | ----------------------------------------------------
id             |       uuid | _required_ | The room identifier.
audience       |     string | _required_ | The audience of the room.
source_room_id |       uuid | _optional_ | The identifier of the source room for derived rooms.
time           | [int, int] | _required_ | Opening and closing timestamps in seconds.
tags           |       json | _optional_ | Tags object associated with the room.
created_at     |        int | _required_ | Room creation timestamp in seconds.
