import Foundation
import Testing
@testable import ProxymanCloneApp

@Test func decodesEngineStatus() throws {
    let data = Data(#"{"protocol_version":1,"proxy_state":"stopped","listen_address":null,"captured_transactions":0}"#.utf8)
    let status = try JSONDecoder().decode(EngineStatus.self, from: data)

    #expect(status.protocolVersion == 1)
    #expect(status.proxyState == "stopped")
    #expect(status.listenAddress == nil)
    #expect(status.capturedTransactions == 0)
}
