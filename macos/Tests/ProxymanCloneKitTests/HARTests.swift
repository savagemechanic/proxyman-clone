import Foundation
import Testing
@testable import ProxymanCloneKit

@Test func exportsRepresentativeTransactionAsHAR12() throws {
    let transaction = CapturedTransaction(
        id: 42,
        startedAtUnixMs: 1_788_888_000_123,
        scheme: "https",
        host: "api.example.com",
        method: "POST",
        target: "/v1/items?limit=2",
        requestHeaders: [
            HeaderField(name: "Content-Type", value: "application/json"),
            HeaderField(name: "Cookie", value: "session=abc; theme=dark")
        ],
        requestBodyBytes: 11,
        requestBodyPreview: BodyPreview(
            contentType: "application/json",
            text: #"{"ok":true}"#,
            capturedBytes: 11,
            totalBytes: 11,
            truncated: false
        ),
        statusCode: 201,
        responseHeaders: [
            HeaderField(name: "Content-Type", value: "application/json"),
            HeaderField(name: "Set-Cookie", value: "id=xyz; Path=/")
        ],
        responseBodyBytes: 13,
        responseBodyPreview: BodyPreview(
            contentType: "application/json",
            text: #"{"id":"xyz"}"#,
            capturedBytes: 12,
            totalBytes: 13,
            truncated: true
        ),
        state: "complete"
    )

    let document = HARExporter.document(from: [transaction], creatorVersion: "test")
    #expect(document.log.version == "1.2")
    #expect(document.log.creator.name == "Proxyman Clone")
    #expect(document.log.entries.count == 1)

    let entry = try #require(document.log.entries.first)
    #expect(entry.request.url == "https://api.example.com/v1/items?limit=2")
    #expect(entry.request.queryString == [HARNameValue(name: "limit", value: "2")])
    #expect(entry.request.cookies.contains(HARNameValue(name: "session", value: "abc")))
    #expect(entry.request.postData?.mimeType == "application/json")
    #expect(entry.response.status == 201)
    #expect(entry.response.cookies == [HARNameValue(name: "id", value: "xyz")])
    #expect(entry.response.content.comment?.contains("truncated") == true)

    let data = try HARExporter.data(from: [transaction], creatorVersion: "test")
    let decoded = try JSONDecoder().decode(HARDocument.self, from: data)
    #expect(decoded == document)
}

@Test func incompleteTransactionExportsStatusZero() throws {
    let transaction = CapturedTransaction(
        id: 1,
        startedAtUnixMs: 1_788_888_000_000,
        scheme: "http",
        host: "localhost:8081",
        method: "GET",
        target: "/health",
        requestHeaders: [],
        requestBodyBytes: 0,
        requestBodyPreview: nil,
        statusCode: nil,
        responseHeaders: [],
        responseBodyBytes: 0,
        responseBodyPreview: nil,
        state: "failed"
    )

    let entry = try #require(HARExporter.document(from: [transaction]).log.entries.first)
    #expect(entry.response.status == 0)
    #expect(entry.response.statusText == "failed")
    #expect(entry.comment?.contains("failed") == true)
}
