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

@Test func decodesCapturedTransaction() throws {
    let data = Data(#"{"id":7,"started_at_unix_ms":1234,"scheme":"https","host":"example.com","method":"GET","target":"/v1/test","request_headers":[{"name":"Accept","value":"application/json"}],"request_body_bytes":0,"status_code":200,"response_headers":[{"name":"Content-Type","value":"application/json"}],"response_body_bytes":42,"state":"complete"}"#.utf8)
    let transaction = try JSONDecoder().decode(CapturedTransaction.self, from: data)

    #expect(transaction.id == 7)
    #expect(transaction.scheme == "https")
    #expect(transaction.statusCode == 200)
    #expect(transaction.responseBodyBytes == 42)
    #expect(transaction.responseHeaders.first?.name == "Content-Type")
}
