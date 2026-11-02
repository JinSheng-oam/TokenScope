# syntax=docker/dockerfile:1.7
FROM golang:1.23-alpine AS build
WORKDIR /src
RUN apk add --no-cache ca-certificates git
COPY hub/go.mod ./hub/
WORKDIR /src/hub
RUN --mount=type=cache,target=/go/pkg/mod go mod download
COPY hub/ ./
RUN --mount=type=cache,target=/go/pkg/mod CGO_ENABLED=0 go build -trimpath -ldflags="-s -w" -o /out/tokenscope-hub ./cmd/hub

FROM alpine:3.21
RUN apk add --no-cache ca-certificates tzdata wget \
 && addgroup -S -g 10001 tokenscope \
 && adduser -S -D -H -u 10001 -G tokenscope tokenscope \
 && mkdir -p /data \
 && chown -R tokenscope:tokenscope /data
COPY --from=build /out/tokenscope-hub /usr/local/bin/tokenscope-hub
USER 10001:10001
EXPOSE 8787
VOLUME ["/data"]
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 CMD wget -q -O /dev/null http://127.0.0.1:8787/healthz || exit 1
ENTRYPOINT ["/usr/local/bin/tokenscope-hub"]
CMD ["serve", "--db", "/data/tokenscope-hub.db", "--listen", "0.0.0.0:8787"]
