import Foundation
import Network

struct EngineStatus: Codable, Equatable {
    let protocolVersion: UInt16
    let proxyState: String
    let listenAddress: String?
    let capturedTransactions: UInt64

    enum CodingKeys: String, CodingKey {
        case protocolVersion = "protocol_version"
        case proxyState = "proxy_state"
        case listenAddress = "listen_address"
        case capturedTransactions = "captured_transactions"
    }
}

enum EngineClientError: Error {
    case invalidResponse
    case engineError(String)
}

actor EngineClient {
    private let host: NWEndpoint.Host
    private let port: NWEndpoint.Port

    init(host: String = "127.0.0.1", port: UInt16 = 9099) {
        self.host = NWEndpoint.Host(host)
        self.port = NWEndpoint.Port(rawValue: port)!
    }

    func fetchStatus() async throws -> EngineStatus {
        let data = try await send(jsonLine: #"{"type":"get_status"}"#)
        let envelope = try JSONSerialization.jsonObject(with: data) as? [String: Any]
        guard let envelope, let type = envelope["type"] as? String else {
            throw EngineClientError.invalidResponse
        }

        if type == "error" {
            throw EngineClientError.engineError(envelope["message"] as? String ?? "Unknown engine error")
        }
        guard type == "status" else {
            throw EngineClientError.invalidResponse
        }
        return try JSONDecoder().decode(EngineStatusEnvelope.self, from: data).status
    }

    private func send(jsonLine: String) async throws -> Data {
        let connection = NWConnection(host: host, port: port, using: .tcp)
        return try await withCheckedThrowingContinuation { continuation in
            connection.stateUpdateHandler = { state in
                switch state {
                case .ready:
                    connection.send(content: Data((jsonLine + "\n").utf8), completion: .contentProcessed { error in
                        if let error {
                            connection.cancel()
                            continuation.resume(throwing: error)
                            return
                        }
                        connection.receive(minimumIncompleteLength: 1, maximumLength: 1_048_576) { data, _, _, error in
                            connection.cancel()
                            if let error {
                                continuation.resume(throwing: error)
                            } else if let data {
                                continuation.resume(returning: data)
                            } else {
                                continuation.resume(throwing: EngineClientError.invalidResponse)
                            }
                        }
                    })
                case .failed(let error):
                    connection.cancel()
                    continuation.resume(throwing: error)
                default:
                    break
                }
            }
            connection.start(queue: .global(qos: .userInitiated))
        }
    }
}

private struct EngineStatusEnvelope: Codable {
    let status: EngineStatus
}
