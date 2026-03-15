console.log("Querying /api/search");
fetch("/api/search", {
    method: 'POST',
    headers: {
        'Content-Type': 'text/plain'
    },
    body: "wow, i am so suprised!!!",
}).then((response) => console.log(response));
