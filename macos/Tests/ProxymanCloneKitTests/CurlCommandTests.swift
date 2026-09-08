import Testing
@testable import ProxymanCloneKit

@Test func curlCommandEscapesQuotesAndPreservesHeaders() throws {
    let transaction = CapturedTransaction(
        id: 1,
        startedAtUnixMs: 0,
        scheme: "https",
        host: "api.example.com",
        method: "POST",
        target: "/search?q=hello world",
        requestHeaders: [
            HeaderField(name: "Authorization", value: "Bearer abc'def"),
            HeaderField(name: "Content-Type", value: "application/json")
        ],
        requestBodyBytes: 17,
        requestBodyPreview: BodyPreview(
            contentType: "application/json",
            text: #"{"name":"O'Reilly"}"#,
            capturedBytes: 17,
            totalBytes: 17,
            truncated: false
        ),
        statusCode: 200,
        responseHeaders: [],
        responseBodyBytes: 0,
        responseBodyPreview: nil,
        state: "complete"
    )

    let command = try CurlCommand.generate(from: transaction)
    #expect(command.contains("curl -X 'POST'"))
    #expect(command.contains("'https://api.example.com/search?q=hello world'"))
    #expect(command.contains("Authorization: Bearer abc'\"'\"'def"))
    #expect(command.contains("--data-raw"))
    #expect(command.contains("O'\"'\"'Reilly"))
}

@Test func curlCommandRejectsTruncatedRequestBody() {
    let transaction = CapturedTransaction(
        id: 2,
        startedAtUnixMs: 0,
        scheme: "http",
        host: "localhost:8080",
        method: "POST",
        target: "/upload",
        requestHeaders: [],
        requestBodyBytes: 100_000,
        requestBodyPreview: BodyPreview(
            contentType: "text/plain",
            text: "partial",
            capturedBytes: 7,
            totalBytes: 100_000,
            truncated: true
        ),
        statusCode: nil,
        responseHeaders: [],
        responseBodyBytes: 0,
        responseBodyPreview: nil,
        state: "pending"
    )

    #expect(throws: CurlCommandError.truncatedRequestBody) {
        try CurlCommand.generate(from: transaction)
    }
}

@Test func curlCommandAllowsBodylessRequests() throws {
    let transaction = CapturedTransaction(
        id: 3,
        startedAtUnixMs: 0,
        scheme: "https",
        host: "example.com",
        method: "GET",
        target: "/",
        requestHeaders: [],
        requestBodyBytes: 0,
        requestBodyPreview: nil,
        statusCode: 204,
        responseHeaders: [],
        responseBodyBytes: 0,
        responseBodyPreview: nil,
        state: "complete"
    )

    let command = try CurlCommand.generate(from: transaction)
    #expect(command == "curl -X 'GET' 'https://example.com/'")
}
