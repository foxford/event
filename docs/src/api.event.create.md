# Create

Create an Event.

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
method           | string | _required_ | Always `event.create`.
response_topic   | string | _required_ | Always `agents/${ME}/api/v1/in/${APP_NAME}`.
correlation_data | string | _required_ | The same value will be in a response.

**Payload**

Name        | Type       | Default    | Description
----------- | ---------- | ---------- | ------------------
room_id     | uuid       | _required_ | The room identifier. The room must be opened. The agent must be entered to the room.
type        | string     | _required_ | Event type.
set         | string     |       type | Collection set name. Equals to `type` parameter by default.
label       | string     | _optional_ | Collection item label.
data        | json       | _required_ | The event JSON data.

**Response**

If successful, the response payload contains an **Event** object.
