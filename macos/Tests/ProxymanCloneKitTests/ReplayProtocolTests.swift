import Foundation
import Testing
@testable import ProxymanCloneKit

private struct ReplayCommand: Codable, Equatable {
    let type: String
    let id: UInt64
}

private struct ReplayEnvelope: Codable {
    let type: String
    let sourceTransactionID: UInt64
    let transaction: CapturedTransaction

    enum CodingKeys: String, CodingKey {
        case type
        case sourceTransactionID = "source_transaction_id"
        case transaction
    }
}

@Test func replayCommandUsesVersionedControlShape() throws {
    let command = ReplayCommand(type: "replay_transaction", id: 42)
    let data = try JSONEncoder().encode(command)
    let json = try #require(String(data: data, encoding: .utf8))

    #expect(json.contains(#""type":"replay_transaction""#))
    #expect(json.contains(#""id":42"#))
}

@Test func replayResultDecodesCapturedTransaction() throws {
    let data = Data(#"{"type":"replay_result","source_transaction_id":7,"transaction":{"id":8,"started_at_unix_ms":1234,"scheme":"https","host":"example.com","method":"GET","target":"/v1/users","request_headers":[],"request_body_bytes":0,"request_body_preview":{"content_type":null,"text":"","captured_bytes":0,"total_bytes":0,"truncated":false},"status_code":200,"response_headers":[],"response_body_bytes":2,"response_body_preview":{"content_type":"text/plain","text":"ok","captured_bytes":2,"total_bytes":2,"truncated":false},"state":"complete"}}"#.utf8)
    let envelope = try JSONDecoder().decode(ReplayEnvelope.self, from: data)

    #expect(envelope.type == "replay_result")
    #expect(envelope.sourceTransactionID == 7)
    #expect(envelope.transaction.id == 8)
    #expect(envelope.transaction.statusCode == 200)
    #expect(envelope.transaction.responseBodyPreview?.text == "ok")
}
