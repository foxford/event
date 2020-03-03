# state.read

Read a [state](../state.md#state) of _sets_ at a [room](../room.md#room).

## Authorization

The tenant authorizes the current _agent_ for a `list` action on `["rooms", room_id, "list"]` object.

## Multicast request

Name            | Type     | Default    | Description
--------------- | -------- | ---------- | -------------------------------------------------------
room_id         | string   | _required_ | The room's identifier.
sets            | [string] | _required_ | Set's names to calculate the state for. Up to 10 elements.
occurred_at     | int      |        now | The number of milliseconds since the room opening to specify the moment of state calculation.
last_created_at | int      | _optional_ | `created_at` value of the last seen event for pagination in milliseconds.
limit           | int      |        100 | Limits the number of events in the response.

## Unicast response

**Status:** 200.

**Payload:** [state](../state.md#state) object. If `sets` parameter has only one element, `has_next` key appears with a boolean value indicating that there are more data left for pagination
when `true`.
