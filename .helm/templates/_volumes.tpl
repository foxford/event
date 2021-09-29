{{- define "volumes" }}
- name: config
  configMap:
    name: {{ .Chart.Name }}-config
- name: internal
  secret:
    secretName: secrets-internal
{{- end }}

{{- define "volumeMounts" }}
- name: config
  mountPath: /app/App.toml
  subPath: App.toml
- name: internal
  mountPath: {{ printf "/app/%s" (pluck .Values.werf.env .Values.app.id_token.key | first | default .Values.app.id_token.key._default) }}
  subPath: private_key
{{- end }}