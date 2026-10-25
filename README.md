# mnhr.org

mnhr.org URL shortener service

Requires Rust 1.88 or newer. Dependencies use Cargo's default registry configuration,
including the `menhera-cooldown` source replacement when configured.

Copy `example.env` to `.env`, configure the service, and run `cargo run --locked`.
The service searches the working directory and its parents for `.env`. Existing
environment variables take precedence over file values; variable substitution
in `.env` is enabled.

## License

Licensed under the Mozilla Public License, version 2.0.
