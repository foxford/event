# State

A _state_ is an object of aggregated [events](api.event.md#event).
The purpose is to represent the actual condition of the [room](api.room.md#room) on the given moment
without listing unrelated _events_.

The grouping is being performed by two levels: _set_ and _label_.
I.e. at first _events_ are being grouped by _set_ which stands for collection and then by _label_
inside of each _set_.

## Sets by example

_Sets_ may be hard to understand on abstract level but given an example it's pretty simple.

Say, there is a bunch of text message _events_ that have _set_ = `messages`. This means that
there's `messages` collection. Messages may be added, edited or removed. Identify of a message
is tracked through _label_.

When [reading](api.state.read.md) the state of this say for the current moment the state contains
a paginated `messages` collection grouped by _label_ with the latest versions of each message.

## Non-collection states

If there's a need to track a single entity one may simply use a _set_ with a single element by
specifying a constant _label_. This is the case for example for tracking current UI layout mode.

## Properties

A _state_ is a dynamic JSON object the structure of which depend on _sets_ requested.

The general structure has _set_ names as keys and arrays of latest _events_ for each _label_
as values. For example:

```json
{
    "messages": [
      {
        "id": "c28d5544-52a5-11ea-9b6f-60f81db6d53e",
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

where `messages` is _set_ and `message-1` is _label_.
