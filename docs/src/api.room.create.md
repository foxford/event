# Create

Create an events Room.

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
method           | string | _required_ | Always `room.create`.
response_topic   | string | _required_ | Always `agents/${ME}/api/v1/in/${APP_NAME}`.
correlation_data | string | _required_ | The same value will be in a response.

**Payload**

Name        | Type       | Default    | Description
----------- | ---------- | ---------- | ------------------
audience    | string     | _required_ | The room audience.
time        | [int, int] | _required_ | A [lt, rt) range of unix time (seconds) or null (unbounded).
tags        | json       | _optional_ | Tenant-specific JSON object associated with the room.

**Response**

If successful, the response payload contains a **Room** object.
