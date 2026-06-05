Beam-init uses the tokio async runtime. On this in addition to the main event loop it runs two tasks that produce events on a channel:

* The signalfd reader: responsible for getting child exit (`SIGCHLD`) notifications.
* The api server: responsible for listening for HTTP requests on `/run/beam-init`.

The main event loop reads these events from the channel and which updates the `ServiceManager` state as appropriate and takes any other necessary action like producing a response to an api request or spawning a process.

There is also a log reader instance per service which directly writes the logs to the `Logs` instance after reading them from the stdout/stderr pipe without going through the main event loop.


```mermaid
flowchart LR
    subgraph beam-init
    S[signalfd reader] -->|signal| M[main event loop]
    pipe -->|reads| L[log reader]
    A[api server] -->|command| M
    M -->|response| A
    end

    M -->|spawn| P[service]
    P[service] -->|writes| pipe
    P -->|exit| S

    beamctl -->|http request| A
    A -->|http response| beamctl
```
