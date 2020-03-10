# edition.list

List all [editions](../edition.md#edition) belonging to a given source room.

## Authorization

The tenant authorizes the current _agent_ for `update` action on `["rooms", room_id]` object.

## Multicast request

Name            | Type       | Default    | Description
--------------- | ---------- | ---------- | ------------------------------------------------------------
room_id         | uuid       | _required_ | Source room for which to list the editions.
last_created_at | int        | _optional_ | `last_created_at` value of the last seen edition on the previous page
limit           | int        |        25  | Limits the number of editions listed in the response.


## Unicast response

**Status:** 200.

**Payload:** list of [edition](../edition.md#edition) objects.
