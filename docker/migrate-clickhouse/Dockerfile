FROM migrate/migrate AS migrate
FROM alpine:3.21
RUN apk add --no-cache gettext
COPY --from=migrate /migrate /usr/local/bin/migrate
COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh
ENTRYPOINT ["/entrypoint.sh"]
