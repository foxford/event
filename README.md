# Event

An event service.


## Development

This services requires both [mqtt-gateway][mqtt-gateway] and legacy [events-api][events-api] service.

The legacy is needed because currently this service is being developed as an adapter to the legacy
one but in the future all of its functionality will be reimplemented here.

For development convenience all of the dependiences are wrapped into a docker-compose project:

```bash
export COMPOSE_PROJECT_NAME=event
export COMPOSE_FILE=docker/docker-compose
docker-compose up
```

This will start up the broker on port 1883.

Also this service requires local postgres with a database created and migrated:

```bash
createdb event.dev

export DATABASE_URL=postgres://postgres@localhost/event.dev
diesel migration run
```

The you can build and run the service locally having stable Rust [installed][rustup]:

```bash
cargo run
```

[mqtt-gateway]:[http://github.com/netology-group/mqtt-gateway]
[events-api]:[http://github.com/netology-group/ulms-events-api]
[rustup]:[https://rustup.rs]


## Deployment

This service has a regular deployment to k8s with skaffold.
For example to deploy the current revision to testing run:

```bash
NAMESPACE=testing ./deploy.init.sh
IMAGE_TAG=$(git rev-parse --short HEAD) skaffold run -n testing
```


## License

The source code is provided under the terms of [the MIT license][license].

[license]:http://www.opensource.org/licenses/MIT
