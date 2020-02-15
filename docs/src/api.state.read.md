# Read

Read state.

**Request**

```bash
pub agents/${ME}/api/v1/out/${APP_NAME}
```

**Topic parameters**

Name     | Type   | Default    | Description
-------- | ------ | ---------- | ------------------
ME       | string | _required_ | Agent identifier.
APP_NAME | string | _required_ | Name of the application.

**Properties**

Name             | Type   | Default    | Description
---------------- | ------ | ---------- | ------------------
type             | string | _required_ | Always `request`.
method           | string | _required_ | Always `state.read`.
response_topic   | string | _required_ | Always `agents/${ME}/api/v1/in/${APP_NAME}`.
correlation_data | string | _required_ | The same value will be in a response.

**Payload**

Name       | Type       | Default    | Description
---------- | ---------- | ---------- | ------------------
room_id    | string     | _required_ | Returns only events that belong to the room.
sets       | array(string) | _required_ | Set names to calculate the state for. 1 to 10 elements.
occured_at | int        | _required_ | Number of milliseconds since room opening to specify the moment to calculate the state for.
last_created_at | int   | _optional_ | `created_at` value of the last seen event for pagination in milliseconds.
direction  | string     |    forward | Pagination direction: forward | backward.
limit      | int        |        100 | Limits the number of events in the response.

**Response**

If successful, the response payload contains the state object with keys of `sets` parameter items
and values of arrays of **Event** objects.

If `sets` parameter has only one element there will also be `has_next` key with boolean value
indicating that there's more data left for pagination when `true`.
