# change.create

Create an [change](../change.md#change).

## Authorization

The tenant authorizes the current _agent_ for `update` action on `["rooms", room_id]` object.

## Multicast request

Name        | Type     | Default    | Description
----------- | -------- | ---------- | ------------------------------------------------------------
edition_id  | uuid     | _required_ | The edition this change belongs to.
type        | string   | _required_ | Change type, `addition`, `modification` or `removal`
event       | json     | _required_ | Object containing `event_id` and/or event related parameters (`type`, `set`, `label`, `data`, `occurred_at`, `created_by`)

`event` object in case of `addition` type:

Name            | Type         | Default    | Description
--------------- | ------------ | ---------- | ---------------------------------
type            | string       | _required_ | New event type.
set             | string       |       type | New event set.
label           | string       | _optional_ | New event label.
data            | json         | _required_ | New event data.
occurred_at     | int          | _required_ | New event occured at, timestamp in nanoseconds.
created_by      | agent_id     | _required_ | New event created by.

`event` object in case of `modification` type:

Name            | Type         | Default    | Description
--------------- | ------------ | ---------- | ---------------------------------
event_id        | uuid         | _required_ | Event identifier to update during commit.
type            | string       | _optional_ | New type to update event with.
set             | uuid         | _optional_ | New set to update event with.
label           | uuid         | _optional_ | New label to update event with.
data            | json         | _optional_ | New data to update event with.
occurred_at     | int          | _optional_ | New occurred_at to update event with, timestamp in nanoseconds.
created_by      | agent_id     | _optional_ | New created_by to update event with.

If any property is set - it overrides corresponding event's property during commit.

`event` object in case of `removal` type:

Name            | Type         | Default    | Description
--------------- | ------------ | ---------- | ---------------------------------
event_id        | uuid         | _required_ | Event identifier to skip during commit.


## Unicast response

**Status:** 201.

**Payload:** [change](../change.md#change) object.

## Example

    Creates change that will delete event with id = "654e2131-e89b-4296-99a5-9f8acc432465"

    ```json
    {
        "edition_id": "123e4567-e89b-4296-99a5-9f8acc922799",
        "type": "removal",
        "event": {
            "event_id": "654e2131-e89b-4296-99a5-9f8acc432465",
        }
    }
    ```
