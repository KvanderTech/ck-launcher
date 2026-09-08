#include "backend.h"
#include <QtTest>
class ProtocolTest : public QObject {
    Q_OBJECT
  private slots:
    void frames() {
        QJsonObject result;
        const QByteArray reply("{\"id\":1,\"result\":[]}");
        const QByteArray error("{\"id\":1,\"error\":{\"code\":\"busy\"}}");
        const QByteArray event("{\"event\":\"launcher://progress\",\"data\":{}}");
        QVERIFY(Backend::decode(reply, &result));
        QVERIFY(Backend::decode(error, &result));
        QVERIFY(Backend::decode(event, &result));
    }
    void rejectsAmbiguity() {
        QJsonObject result;
        QList<QByteArray> frames;
        frames << "[]" << "null" << "{bad}" << "{\"id\":1,\"result\":0,\"error\":{}}"
               << "{\"id\":\"1\",\"result\":0}" << "{\"event\":7,\"data\":{}}"
               << "{\"id\":1,\"error\":\"failed\"}" << "{\"id\":1,\"error\":null}"
               << "{\"id\":1,\"error\":{}}" << "{\"id\":1.5,\"result\":0}"
               << "{\"id\":-1,\"result\":0}" << "{\"id\":1e99,\"result\":0}";
        for (const auto &frame : frames) {
            QVERIFY(!Backend::decode(frame, &result));
        }
    }
    void rejectsOversize() {
        QJsonObject result;
        QVERIFY(!Backend::decode(QByteArray(8 * 1024 * 1024 + 1, ' '), &result));
    }
};
QTEST_MAIN(ProtocolTest)
#include "protocol.moc"
