// Check 11: a child frame that loaded says so.
parent.postMessage({ type: "redteam:child", how: location.hash.slice(1) }, "*")
