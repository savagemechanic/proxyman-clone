import Foundation
import Testing
@testable import ProxymanCloneKit

@Test func composedRequestEncodesForEngineProtocol() throws {
    let request = ComposedRequest(
        scheme: "https",
        host: "api.example.com",
        port: 8443,
        method: "POST",
        target: "/v1/items",
        headers: [HeaderField(name: "Content-Type", value: "application/json")],
        body: "{\"ok\":true}"
    )

    let data = try JSONEncoder().encode(request)
    let json = try #require(String(data: data, encoding: .utf8))
    #expect(json.contains(#""scheme":"https""#))
    #expect(json.contains(#""host":"api.example.com""#))
    #expect(json.contains(#""port":8443"#))
    #expect(json.contains(#""body":"{\"ok\":true}""#))
    #expect(try JSONDecoder().decode(ComposedRequest.self, from: data) == request)
}

@Test func composerPrefillDerivesCustomPortAndDropsFramingHeaders() throws {
    let transaction = capturedTransaction(
        requestHeaders: [
            HeaderField(name: "Host", value: "api.example.com:8443"),
            HeaderField(name: "Connection", value: "keep-alive"),
            HeaderField(name: "Content-Length", value: "7"),
            HeaderField(name: "Content-Type", value: "application/json"),
            HeaderField(name: "X-Debug", value: "1")
        ],
        requestBodyBytes: 7,
        requestBodyPreview: BodyPreview(
            contentType: "application/json",
            text: "{\"x\":1}",
            capturedBytes: 7,
            totalBytes: 7,
            truncated: false
        )
    )

    let prefill = try #require(ComposerPrefill(transaction: transaction))
    #expect(prefill.host == "api.example.com")
    #expect(prefill.port == 8443)
    #expect(prefill.body == "{\"x\":1}")
    #expect(prefill.warning == nil)
    #expect(prefill.headers.contains(HeaderField(name: "Content-Type", value: "application/json")))
    #expect(prefill.headers.contains(HeaderField(name: "X-Debug", value: "1")))
    #expect(prefill.headers.allSatisfy { header in
        !["host", "connection", "content-length"].contains(header.name.lowercased())
    })
}

@Test func composerPrefillWarnsInsteadOfInventingTruncatedBody() throws {
    let transaction = capturedTransaction(
        requestHeaders: [
            HeaderField(name: "Host", value: "api.example.com"),
            HeaderField(name: "Content-Type", value: "text/plain")
        ],
        requestBodyBytes: 20,
        requestBodyPreview: BodyPreview(
            contentType: "text/plain",
            text: "partial",
            capturedBytes: 7,
            totalBytes: 20,
            truncated: true
        )
    )

    let prefill = try #require(ComposerPrefill(transaction: transaction))
    #expect(prefill.body == nil)
    #expect(prefill.warning != nil)
}

@Test func composerPrefillPreservesExplicitEmptyBody() throws {
    let transaction = capturedTransaction(
        requestHeaders: [
            HeaderField(name: "Host", value: "example.com"),
            HeaderField(name: "Content-Length", value: "0")
        ],
        requestBodyBytes: 0,
        requestBodyPreview: BodyPreview(
            contentType: nil,
            text: "",
            capturedBytes: 0,
            totalBytes: 0,
            truncated: false
        )
    )

    let prefill = try #require(ComposerPrefill(transaction: transaction))
    #expect(prefill.body == "")
}

@Test func tunnelTransactionCannotPrefillComposer() {
    var transaction = capturedTransaction(
        requestHeaders: [],
        requestBodyBytes: 0,
        requestBodyPreview: nil
    )
    transaction = CapturedTransaction(
        id: transaction.id,
        startedAtUnixMs: transaction.startedAtUnixMs,
        scheme: "tunnel",
        host: transaction.host,
        method: "CONNECT",
        target: "example.com:443",
        requestHeaders: transaction.requestHeaders,
        requestBodyBytes: transaction.requestBodyBytes,
        requestBodyPreview: transaction.requestBodyPreview,
        statusCode: transaction.statusCode,
        responseHeaders: transaction.responseHeaders,
        responseBodyBytes: transaction.responseBodyBytes,
        responseBodyPreview: transaction.responseBodyPreview,
        state: transaction.state
    )
    #expect(ComposerPrefill(transaction: transaction) == nil)
}

private func capturedTransaction(
    requestHeaders: [HeaderField],
    requestBodyBytes: UInt64,
    requestBodyPreview: BodyPreview?
) -> CapturedTransaction {
    CapturedTransaction(
        id: 11,
        startedAtUnixMs: 1,
        scheme: "https",
        host: "api.example.com",
        method: "POST",
        target: "/v1/items",
        requestHeaders: requestHeaders,
        requestBodyBytes: requestBodyBytes,
        requestBodyPreview: requestBodyPreview,
        statusCode: 200,
        responseHeaders: [],
        responseBodyBytes: 0,
        responseBodyPreview: nil,
        state: "complete"
    )
}
