# List

List events.

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
method           | string | _required_ | Always `agent.list`.
response_topic   | string | _required_ | Always `agents/${ME}/api/v1/in/${APP_NAME}`.
correlation_data | string | _required_ | The same value will be in a response.

**Payload**

Name       | Type       | Default    | Description
---------- | ---------- | ---------- | ------------------
room_id    | string     | _required_ | Returns only events that belong to the room.
type       | string     | _optional_ | Event type filter.
last_id    | string     | _optional_ | Last seen id for pagination.
direction  | string     |    forward | Pagination direction: forward | backward.
limit      | int        |        100 | Limits the number of events in the response.

**Response**

If successful, the response payload contains the list of **Event** objects.
