# Room

The room is a scope for storing [events](event.md#event) and sharing them between
[agents](agent.md#agent).

The room belongs to an _audience_ that is a scope of a tenant. So the tenant controls the _audience_ that
contains multiple rooms. The tenant also may associate some arbitrary _tags_ with the room to keep the
relations with its internal entities.

The room has a _time_ period when it is open, i.e. available for creating events.

The room may be derived from another room during the [adjustment](room/adjust.md).
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
