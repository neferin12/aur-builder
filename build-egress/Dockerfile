FROM alpine:3.22
RUN apk add --no-cache squid
COPY squid.conf /etc/squid/squid.conf
USER squid
ENTRYPOINT ["squid", "-N", "-f", "/etc/squid/squid.conf"]
