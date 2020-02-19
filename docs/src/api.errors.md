# Errors

In case of error the response payload is a RFC7807 Problem Details object:

Name   | Type   | Default    | Description
------ | ------ | ---------- | ---------------------------------
type   | string | _required_ | Error type.
title  | string | _requried_ | Human-readable short description.
detail | string | _optional_ | Detailed error description.
status | int    | _required_ | HTTP-compatible status code. The same code is in response properties.

## Troubleshooting by status code

- **400 Bad Request** – Failed to parse JSON payload of the message or endpoint-specific validation failed.
- **403 Forbidden** – Authorization failed. Check out Authorization section of the endpoint.
- **404 Not Found** – The entity doesn't exist in the DB or expired.
- **405 Method Not Allowed** – Unknown `method` property value in the request.
- **422 Unprocessable Entity** – DB query error or some logic error.
- **500 Internal Server Error** – See Sentry for details.
