console.log("Querying /api/search");
fetch("/api/search", {
    method: 'POST',
    headers: {
        'Content-Type': 'text/plain'
    },
    body: "very bad movie",
}).then((response) => console.log(response));
