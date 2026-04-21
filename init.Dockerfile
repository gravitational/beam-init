FROM python:3.12-alpine
COPY zombie.py .

RUN apk add dumb-init
ENTRYPOINT [ "/usr/bin/dumb-init", "--", "sleep", "1000" ]
