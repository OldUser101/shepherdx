# `shepherd-app`

- [x] control router publishes mqtt messages
- [x] files router files saved via sheep
- [x] upload router for python & zip upload

# `shepherd-common`

- [ ] fully centralised configuration, partial impl.

# `shepherd-mqtt`

- [x] structured subscriptions
- [x] structured publications
- [ ] unsubscribing?

# `shepherd-run`

- [ ] handles events from gpio (start button), impl. untested
- [x] handles events from mqtt
- [x] sets up hopper for usercode (log + start)
- [x] copies initial image to tmp
- [ ] hardware reset, probably via external scripting?
- [ ] usercode setup and management, impl. needs extensive testing
- [x] sending start info to usercode via hopper
- [x] internal usercode state tracking

# `shepherd-ws`

- [x] handle incoming connections as subscriptions to mqtt topics
- [x] handle removal of websocket connections
- [x] hopper for usercode logs and camera
- [x] usercode log and camera message buffering
- [ ] send buffered logs to new websocket clients
- [ ] initial image loading for camera

