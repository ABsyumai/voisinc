ARG CRATE_NAME=discordbot_template
FROM rust:1 as build

# setup environment
WORKDIR /app
RUN apt-get update && apt-get install -y cmake

# cacheing libs
COPY Cargo.toml Cargo.lock ./
RUN mkdir src \
    && echo "fn main(){}" > src/main.rs \
    && cargo build --release

# build binaly
COPY . .
RUN cargo build --release


FROM gcr.io/distroless/cc-debian12:nonroot

WORKDIR /app
COPY --from=build --chown=nonroot:nonroot ./app/target/release/$CRATE_NAME /app

CMD [ "/app" ]
