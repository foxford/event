
# Message handling

`MessageHandler` is responsible for handling individual requests and events.
It accepts a message as binary array, parses it as MQTT envelope and runs specific handling depending
on the type.

## Handling requests

At first, `MessageHandler` routes the request by `method` property with routes defined with a macro
at `endpoint` module.

Each route lives in a submodule of `endpoint` related to the method name. So for the `event.create`
method, the handler is `CreateHandler` at `endpoint::event` module.

A request handler is an object that implements the `endpoint::RequestHandler` trait. This trait defines
the `Payload` associated type, error title constant and `handle` function.

`MessageHandler` gets `Payload` struct for the handler type that matches the method and parses
the incoming envelope payload to that type.

Then it calls the `handle` method passing the parsed payload there. Also, it passes the `Context` object
which shares common resources like configuration, DB connection pool, authz object, etc.
Along with the `Context`, it passes message properties and handling start timestamp.
The latter is necessary for calculating the timing properties of outgoing messages.

The handler returns a vector of outgoing messages. Usually, it's a response and maybe a broadcast
notification. `MessageHandler` publishes these messages then.

## Handling events

That is similar to handling requests, but instead of `method` property the routing is being
done by `label` property and the endpoint trait is `EventHandler` which has a slightly different
signature but the point is the same.

## Error handling

In case of error, endpoint handlers return `Error` structure from [svc_error][svc_error] crate
usually aliased as `SvcError`. This structure represents an RFC7808 Problem Details object containing
both error description and response status code. In can be built with
[SvcError::builder][svc_error_builder] or casted from a [failure::Error][failure_error] struct
with `status` method on `Result<T, failure::Error>`.

The returned error is being used by `MessageHandler` to construct an error response message.

[svc_error]:https://github.com/netology-group/svc-error-rs
[svc_error_builder]:https://docs.rs/svc-error/0.1.8/svc_error/struct.Builder.html
[failure_error]:https://docs.rs/failure/0.1.7/failure/struct.Error.html
