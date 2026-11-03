// Check 9: what the CSP doesn't cover, link hints and WebRTC, aimed at the
// side listeners. Measured per engine, never a failure.
;(async () => {
  const info = await RT.info
  for (const rel of ["prefetch", "preconnect", "dns-prefetch"]) {
    document.head.append(Object.assign(document.createElement("link"), { rel, href: `${info.side}/side/${rel}` }))
  }
  const out = {}
  // Picture-in-picture: a window over every other, of the page's pixels (Permissions-Policy denies it).
  out.pip = await RT.attempt(async () => {
    const allowed = document.featurePolicy ? document.featurePolicy.allowsFeature("picture-in-picture") : "no featurePolicy"
    return `policy ${allowed}, enabled ${document.pictureInPictureEnabled ?? "unknown"}`
  }, 1000)
  out.webrtc = await RT.attempt(async () => {
    if (typeof RTCPeerConnection !== "function") return "no RTCPeerConnection"
    const pc = new RTCPeerConnection({ iceServers: [{ urls: `stun:127.0.0.1:${info.udp}` }] })
    const kinds = []
    pc.onicecandidate = (e) => e.candidate && kinds.push(e.candidate.type || "?")
    pc.createDataChannel("redteam")
    await pc.setLocalDescription(await pc.createOffer())
    await RT.wait(2500)
    pc.close()
    return kinds.join(", ") || "no candidates"
  }, 5000)
  await RT.wait(1000)
  await RT.report(out)
})()
