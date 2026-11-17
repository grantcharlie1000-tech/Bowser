// Points the cross-origin iframe at the second fixture origin (port injected via the query string).
var port = new URLSearchParams(location.search).get("crossPort");
if (port) document.getElementById("cross").src = "http://localhost:" + port + "/frame.html";
