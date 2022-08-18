# change.list

List all [changes](../edition.md#edition) belonging to a given edition.

## Authorization

The tenant authorizes the current _agent_ for `update` action on `["classrooms", classroom_id]` object.

## Multicast request

Name            | Type       | Default    | Description
--------------- | ---------- | ---------- | ------------------------------------------------------------
id              | uuid       | _required_ | Edition for which to list the editions.
last_created_at | int        | _optional_ | `last_created_at` value of the last seen change on the previous page
limit           | int        |        25  | Limits the number of change listed in the response.


## Unicast response

**Status:** 200.

**Payload:** list of [change](../change.md#change) objects.
