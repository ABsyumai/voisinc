.PHONY: all amd64-gnu arm64-gnu amd64-musl arm64-musl amd64 arm64


amd64-gnu:
	docker build . -t test

arm64-gnu:
	docker buildx build --platform linux/arm64 -t test --load .

amd64-musl:
	docker build --platform linux/amd64 --build-context messense/rust-musl-cross:amd64-musl=docker-image://messense/rust-musl-cross:x86_64-musl -t helloworld:latest  -f Dockerfile.musl .

arm64-musl:
	docker build --platform linux/arm64 --build-context messense/rust-musl-cross:arm64-musl=docker-image://messense/rust-musl-cross:aarch64-musl -t helloworld:latest  -f Dockerfile.musl .


amd64: amd64-gnu amd64-musl

arm64: arm64-gnu arm64-musl
