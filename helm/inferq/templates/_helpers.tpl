{{/*
Expand the name of the chart.
*/}}
{{- define "inferq.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Create a default fully qualified app name.
We truncate at 63 chars because some Kubernetes name fields are limited to this.
If release name contains chart name it will be used as a full name.
*/}}
{{- define "inferq.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{/*
Create chart label value.
*/}}
{{- define "inferq.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Common labels applied to all resources.
*/}}
{{- define "inferq.labels" -}}
helm.sh/chart: {{ include "inferq.chart" . }}
{{ include "inferq.selectorLabels" . }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{/*
Selector labels for the inferq backend pod.
*/}}
{{- define "inferq.selectorLabels" -}}
app.kubernetes.io/name: {{ include "inferq.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/*
Compute the DATABASE_URL. Uses in-cluster service when postgres.enabled,
otherwise falls back to the user-supplied inferq.env.databaseUrl value.
*/}}
{{- define "inferq.databaseUrl" -}}
{{- if .Values.postgres.enabled }}
{{- printf "postgres://%s:%s@%s-postgres:5432/%s" .Values.postgres.auth.username .Values.postgres.auth.password (include "inferq.fullname" .) .Values.postgres.auth.database }}
{{- else }}
{{- required "inferq.env.databaseUrl is required when postgres.enabled=false" .Values.inferq.env.databaseUrl }}
{{- end }}
{{- end }}

{{/*
Compute the VALKEY_URL. Uses in-cluster service when valkey.enabled,
otherwise falls back to the user-supplied inferq.env.valkeyUrl value.
*/}}
{{- define "inferq.valkeyUrl" -}}
{{- if .Values.valkey.enabled }}
{{- printf "redis://%s-valkey:6379" (include "inferq.fullname" .) }}
{{- else }}
{{- required "inferq.env.valkeyUrl is required when valkey.enabled=false" .Values.inferq.env.valkeyUrl }}
{{- end }}
{{- end }}

{{/*
Compute the CLICKHOUSE_URL. Uses in-cluster service when clickhouse.enabled,
otherwise falls back to the user-supplied inferq.env.clickhouseUrl value.
*/}}
{{- define "inferq.clickhouseUrl" -}}
{{- if .Values.clickhouse.enabled }}
{{- printf "http://%s-clickhouse:8123" (include "inferq.fullname" .) }}
{{- else }}
{{- required "inferq.env.clickhouseUrl is required when clickhouse.enabled=false" .Values.inferq.env.clickhouseUrl }}
{{- end }}
{{- end }}

{{/*
Compute the Kafka broker list for OTel Collector exporters.
Uses the in-cluster Redpanda service when redpanda.enabled,
otherwise uses the user-supplied redpanda.externalBrokers value.
*/}}
{{- define "inferq.kafkaBrokers" -}}
{{- if .Values.redpanda.enabled }}
{{- printf "%s-redpanda:9092" (include "inferq.fullname" .) }}
{{- else }}
{{- required "redpanda.externalBrokers is required when redpanda.enabled=false" .Values.redpanda.externalBrokers }}
{{- end }}
{{- end }}
