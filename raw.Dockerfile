FROM python:3.12-alpine

COPY zombie.py .

ENTRYPOINT [ "/bin/sleep",  "1000" ]
