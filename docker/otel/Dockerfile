# Official OTel image is distroless — no shell or wget.
# Copy the binary into debian:12-slim so healthcheck and debugging work.
FROM otel/opentelemetry-collector-contrib:latest AS otel
FROM debian:12-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends wget ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=otel /otelcol-contrib /otelcol-contrib
EXPOSE 4317 4318 13133
ENTRYPOINT ["/otelcol-contrib"]
