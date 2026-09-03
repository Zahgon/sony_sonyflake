FROM ubuntu:24.04

COPY ./sonyflake_server_v2 /sonyflake_server

RUN useradd -m sonyflake
USER sonyflake

ENTRYPOINT ["/sonyflake_server"]
EXPOSE 8080
