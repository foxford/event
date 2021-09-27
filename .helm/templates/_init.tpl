{{- define "wait_postgres" }}
- name: wait-postgres
  image: {{ .Values.werf.image.alpine }}
  command: ['/bin/sh', '-c', 'while ! getent ahostsv4 {{ pluck .Values.werf.env (index .Values.app.database.host .Values.global.org .Values.global.app) | first | default (index .Values.app.database.host .Values.global.org .Values.global.app)._default }}; do sleep 1; done']
{{- end }}

{{- define "wait_redis" }}
- name: wait-redis
  image: {{ .Values.werf.image.alpine }}
  command: ['/bin/sh', '-c', 'while ! getent ahostsv4 {{ pluck .Values.werf.env (index .Values.app.redis.host .Values.global.org .Values.global.app) | first | default (index .Values.app.redis.host .Values.global.org .Values.global.app)._default }}; do sleep 1; done']
{{- end }}