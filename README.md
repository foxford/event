# Event

[Documentation][documentation]

[documentation]:https://docs.netology-group.services/event/index.html

## Development

Start the broker:

```bash
export COMPOSE_PROJECT_NAME=event
export COMPOSE_FILE=docker/docker-compose
docker-compose up
```

Set up database:

```bash
createdb event.dev

export DATABASE_URL=postgres://postgres@localhost/event.dev
diesel migration run --locked-schema
```

Set up config from the sample:

```bash
cp App.toml.sample App.toml
```

The you can build and run the service locally having stable Rust [installed][rustup]:

```bash
cargo run
```

[rustup]:https://rustup.rs


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
