import Foundation
import Testing
@testable import ProxymanCloneKit

@Test func decodesEngineStatus() throws {
    let data = Data(#"{"protocol_version":1,"proxy_state":"stopped","listen_address":null,"captured_transactions":0,"tls_interception_enabled":true}"#.utf8)
    let status = try JSONDecoder().decode(EngineStatus.self, from: data)

    #expect(status.protocolVersion == 1)
    #expect(status.proxyState == "stopped")
    #expect(status.listenAddress == nil)
    #expect(status.capturedTransactions == 0)
    #expect(status.tlsInterceptionEnabled)
}

@Test func decodesCapturedTransactionWithBodyPreview() throws {
    let data = Data(#"{"id":7,"started_at_unix_ms":1234,"scheme":"https","host":"example.com","method":"POST","target":"/v1/test","request_headers":[{"name":"Content-Type","value":"application/json"}],"request_body_bytes":13,"request_body_preview":{"content_type":"application/json","text":"{\"ok\":true}","captured_bytes":11,"total_bytes":13,"truncated":true},"status_code":200,"response_headers":[{"name":"Content-Type","value":"application/json"}],"response_body_bytes":42,"response_body_preview":{"content_type":"application/json","text":"{\"done\":true}","captured_bytes":13,"total_bytes":42,"truncated":true},"state":"complete"}"#.utf8)
    let transaction = try JSONDecoder().decode(CapturedTransaction.self, from: data)

    #expect(transaction.id == 7)
    #expect(transaction.scheme == "https")
    #expect(transaction.statusCode == 200)
    #expect(transaction.requestBodyPreview?.contentType == "application/json")
    #expect(transaction.requestBodyPreview?.truncated == true)
    #expect(transaction.responseBodyPreview?.totalBytes == 42)
    #expect(transaction.responseHeaders.first?.name == "Content-Type")
}
