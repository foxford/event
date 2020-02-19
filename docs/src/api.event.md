# Event

An _event_ is a JSON document representing a fact that took place in a [room](api.room.md#room).

It has arbitrary _type_ and _data_.

Events may be grouped to _sets_ of elements identified by _label_ for aggregation to
[state](api.state.md#state).

_Events_ are **immutable** by design. Any change is a new event.

For example there may be an _event_ for creating a text message and another _event_ for deleting
this message. Tracking the message identity may be achieved by setting _set_ = `messages` and
_label_ = arbitrary message id.

## Properties

Name       | Type     | Default    | Description
---------- | -------- | ---------- | -------------------------------------------------
id         | uuid     | _required_ | The event identifier.
room_id    | uuid     | _required_ | The room identifier to which the event belongs.
type       | string   | _required_ | The event type.
set        | string   |       kind | The set to which the event is related.
label      | string   | _optional_ | A label to identify an element within the set.
data       | json     | _required_ | Schemaless payload of the event.
occured_at | int      | _required_ | Number of milliseconds since the room's opening when the event took place.
created_by | agent_id | _required_ | An agent who created the event.
created_at | int      | _required_ | Event absolute creation timestamp in milliseconds.

## Stream editing events

The room [adjustment](api.room.adjust.md) algorithm depends on the stream editing events structure.
They must have _type_ = `stream` and _data_ = `{ "cut": "start" }` or `{ "cut": "stop" }`.

This is the only dependence on events specifics in the service.
