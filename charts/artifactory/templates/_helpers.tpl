{{/* [INPUT]: Release 身份和已有 Secret 名称。 */}}
{{/* [OUTPUT]: 稳定资源命名、私有 S3 地址和安全上下文。 */}}
{{/* [POS]: Chart 模板的共享命名边界。 */}}
{{/* [PROTOCOL]: Update this header on change, then check AGENTS.md */}}
{{- define "artifactory.fullname" -}}
{{- default .Release.Name .Values.fullnameOverride | trunc 50 | trimSuffix "-" -}}
{{- end -}}

{{- define "artifactory.labels" -}}
app.kubernetes.io/name: artifactory
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version | quote }}
{{- end -}}

{{- define "artifactory.selector" -}}
app.kubernetes.io/name: artifactory
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end -}}

{{- define "artifactory.credentials" -}}
{{- default (printf "%s-credentials" (include "artifactory.fullname" .)) .Values.secrets.credentials -}}
{{- end -}}

{{- define "artifactory.pki" -}}
{{- default (printf "%s-pki" (include "artifactory.fullname" .)) .Values.secrets.pki -}}
{{- end -}}

{{- define "artifactory.gatewaySecret" -}}
{{- default (printf "%s-gateway" (include "artifactory.fullname" .)) .Values.gateway.secretName -}}
{{- end -}}

{{- define "artifactory.registrySigningSecret" -}}
{{- default (printf "%s-registry-signing" (include "artifactory.fullname" .)) .Values.imageBuilds.signingSecret -}}
{{- end -}}

{{- define "artifactory.s3Endpoint" -}}
{{- if .Values.s3.endpoint -}}
{{- .Values.s3.endpoint -}}
{{- else if .Values.s3.enabled -}}
{{- printf "http://%s-s3:8333" (include "artifactory.fullname" .) -}}
{{- else -}}
{{- fail "s3.endpoint is required when embedded S3 is disabled" -}}
{{- end -}}
{{- end -}}

{{- define "artifactory.containerSecurity" -}}
allowPrivilegeEscalation: false
capabilities:
  drop: [ALL]
readOnlyRootFilesystem: true
{{- end -}}
