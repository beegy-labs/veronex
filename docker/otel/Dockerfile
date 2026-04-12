# Official OTel image is distroless — no shell or wget.
# Copy the binary into alpine so healthcheck and debugging work.
FROM otel/opentelemetry-collector-contrib:0.149.0 AS otel
FROM alpine:3.23
RUN --mount=type=cache,target=/var/cache/apk \
    apk add wget ca-certificates
COPY --from=otel /otelcol-contrib /otelcol-contrib
EXPOSE 4317 4318 13133
ENTRYPOINT ["/otelcol-contrib"]
