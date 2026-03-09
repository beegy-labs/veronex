{{/*
Common labels applied to all resources.
*/}}
{{- define "veronex.labels" -}}
helm.sh/chart: {{ .Chart.Name }}-{{ .Chart.Version | replace "+" "_" }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
app.kubernetes.io/part-of: veronex
{{- end }}

{{/*
Selector labels — veronex API
*/}}
{{- define "veronex.api.selectorLabels" -}}
app.kubernetes.io/name: veronex
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/component: api
{{- end }}

{{/*
Selector labels — veronex-analytics
*/}}
{{- define "veronex.analytics.selectorLabels" -}}
app.kubernetes.io/name: veronex-analytics
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/component: analytics
{{- end }}

{{/*
Selector labels — veronex-web
*/}}
{{- define "veronex.web.selectorLabels" -}}
app.kubernetes.io/name: veronex-web
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/component: web
{{- end }}

{{/*
Selector labels — veronex-agent
*/}}
{{- define "veronex.agent.selectorLabels" -}}
app.kubernetes.io/name: veronex-agent
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/component: agent
{{- end }}

{{/*
Selector labels — otel-collector
*/}}
{{- define "veronex.otel.selectorLabels" -}}
app.kubernetes.io/name: otel-collector
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/component: otel-collector
{{- end }}

{{/*
PostgreSQL DATABASE_URL.
Uses bitnami subchart service name when postgresql.enabled=true,
otherwise falls back to externalPostgresql.* values.
*/}}
{{- define "veronex.postgresUrl" -}}
{{- if .Values.postgresql.enabled -}}
postgres://{{ .Values.postgresql.auth.username }}:{{ .Values.postgresql.auth.password }}@{{ .Release.Name }}-postgresql:5432/{{ .Values.postgresql.auth.database }}
{{- else -}}
postgres://{{ .Values.externalPostgresql.username }}:{{ .Values.externalPostgresql.password }}@{{ .Values.externalPostgresql.host }}:{{ .Values.externalPostgresql.port }}/{{ .Values.externalPostgresql.database }}
{{- end -}}
{{- end }}

{{/*
Valkey VALKEY_URL.
Supports optional auth password and DB index for external Valkey.
Subchart Valkey always uses DB 0.
*/}}
{{- define "veronex.valkeyUrl" -}}
{{- if .Values.valkey.enabled -}}
redis://{{ .Release.Name }}-valkey-master:6379/0
{{- else -}}
{{- if .Values.externalValkey.password -}}
redis://:{{ .Values.externalValkey.password }}@{{ .Values.externalValkey.host }}:{{ .Values.externalValkey.port }}/{{ .Values.externalValkey.db | default 0 }}
{{- else -}}
redis://{{ .Values.externalValkey.host }}:{{ .Values.externalValkey.port }}/{{ .Values.externalValkey.db | default 0 }}
{{- end -}}
{{- end -}}
{{- end }}

{{/*
ClickHouse HTTP URL.
*/}}
{{- define "veronex.clickhouseUrl" -}}
{{- if .Values.clickhouse.enabled -}}
http://{{ .Release.Name }}-clickhouse:8123
{{- else -}}
http://{{ .Values.externalClickhouse.host }}:{{ .Values.externalClickhouse.port }}
{{- end -}}
{{- end }}

{{/*
ClickHouse username.
*/}}
{{- define "veronex.clickhouseUser" -}}
{{- if .Values.clickhouse.enabled -}}
{{ .Values.clickhouse.auth.username }}
{{- else -}}
{{ .Values.externalClickhouse.username }}
{{- end -}}
{{- end }}

{{/*
ClickHouse password.
*/}}
{{- define "veronex.clickhousePassword" -}}
{{- if .Values.clickhouse.enabled -}}
{{ .Values.clickhouse.auth.password }}
{{- else -}}
{{ .Values.externalClickhouse.password }}
{{- end -}}
{{- end }}

{{/*
ClickHouse database name.
*/}}
{{- define "veronex.clickhouseDb" -}}
{{- if .Values.clickhouse.enabled -}}
{{ .Values.clickhouse.auth.database }}
{{- else -}}
{{ .Values.externalClickhouse.database }}
{{- end -}}
{{- end }}

{{/*
MinIO / S3 endpoint URL.
*/}}
{{- define "veronex.s3Endpoint" -}}
{{- if .Values.minio.enabled -}}
http://{{ .Release.Name }}-minio:9000
{{- else -}}
{{ .Values.externalMinio.endpoint }}
{{- end -}}
{{- end }}

{{/*
MinIO / S3 access key.
*/}}
{{- define "veronex.s3AccessKey" -}}
{{- if .Values.minio.enabled -}}
{{ .Values.minio.auth.rootUser }}
{{- else -}}
{{ .Values.externalMinio.accessKey }}
{{- end -}}
{{- end }}

{{/*
MinIO / S3 secret key.
*/}}
{{- define "veronex.s3SecretKey" -}}
{{- if .Values.minio.enabled -}}
{{ .Values.minio.auth.rootPassword }}
{{- else -}}
{{ .Values.externalMinio.secretKey }}
{{- end -}}
{{- end }}

{{/*
MinIO / S3 bucket name.
*/}}
{{- define "veronex.s3Bucket" -}}
{{- if .Values.minio.enabled -}}
{{ .Values.minio.defaultBuckets }}
{{- else -}}
{{ .Values.externalMinio.bucket }}
{{- end -}}
{{- end }}

{{/*
MinIO / S3 region.
*/}}
{{- define "veronex.s3Region" -}}
{{- if .Values.minio.enabled -}}
us-east-1
{{- else -}}
{{ .Values.externalMinio.region }}
{{- end -}}
{{- end }}

{{/*
Redpanda / Kafka broker address (single broker string).
*/}}
{{- define "veronex.redpandaBroker" -}}
{{- if .Values.redpandaEnabled -}}
{{ .Release.Name }}-redpanda:9092
{{- else -}}
{{ .Values.externalRedpanda.brokers }}
{{- end -}}
{{- end }}

{{/*
Secret name — resolves to existing secret, ESO-managed, or chart-managed.
*/}}
{{- define "veronex.secretName" -}}
{{- if .Values.externalSecrets.existingSecretName -}}
{{ .Values.externalSecrets.existingSecretName }}
{{- else -}}
{{ .Release.Name }}-veronex-secrets
{{- end -}}
{{- end }}

{{/*
OTel Collector gRPC endpoint.
*/}}
{{- define "veronex.otelEndpoint" -}}
{{- if .Values.otelCollector.enabled -}}
http://{{ .Release.Name }}-otel-collector:4317
{{- else -}}
{{ .Values.veronex.otel.endpoint }}
{{- end -}}
{{- end }}

{{/*
OTel Collector HTTP endpoint (used by veronex-analytics for OTLP HTTP).
When using external OTel, set veronex.otel.httpEndpoint explicitly.
*/}}
{{- define "veronex.otelHttpEndpoint" -}}
{{- if .Values.otelCollector.enabled -}}
http://{{ .Release.Name }}-otel-collector:4318
{{- else if .Values.veronex.otel.httpEndpoint -}}
{{ .Values.veronex.otel.httpEndpoint }}
{{- else -}}
{{ .Values.veronex.otel.endpoint | replace ":4317" ":4318" }}
{{- end -}}
{{- end }}
