# State

A _state_ is an object of aggregated [events](event.md#event).
The purpose is to represent the actual condition of the [room](room.md#room) on a given moment
without listing unrelated _events_.

## Properties

A _state_ is a dynamic JSON object which structure depends on the _sets_ requested.

The general structure has _set_ names as keys and arrays of latest _events_ for each _label_
as values. For example:

```json
{
    // Simple set: only one element as the single event.
    "leader": {
      "id": "2c4dd428-5368-11ea-b75b-60f81db6d53e",
      "set": "leader",
      "data": {
        "account_id": "johndoe.usr.example.org",
        // …
      },
      // …
    },
    // Collection: the array of events for each element.
    "messages": [
      {
        "id": "c28d5544-52a5-11ea-9b6f-60f81db6d53e",
        "set": "messages",
        "label": "message-1",
        "data": {
          // …
        },
        // …
      },
      // …
    ],
    // …
}
```

See below on how this object is being built.
Check [State calculation](../impl/state_calculation.md) out for implementation details.

## Grouping by sets and labels

Only _events_ with a _set_ present can be at the state. Events are grouped by the _set_.

Element identity inside the _set_ is a _label_. Events inside the state are grouped by the _label_.

## Collections and simple sets

A _collection_ is an array with the actual _event_ for each _label_ in a single _set_.

In case of the absent _label_, the _set_ is considered to be _simple_, i.e. containing only one
element. There's no point of wrapping it into an array, so it goes as a single _event_.

## Event creation from the state perspective

Regarding the _state_ for _events_ [creation](event/create.md), the rules are the following:

1. _Set_ is a key in the _state_ where the event will be. By default it equals to _type_.
1. To get a _set_ as a _collection_ one has to specify _label_.
2. To get a simple _set_ as a single _event_ one has to omit _label_.
