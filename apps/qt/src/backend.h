#pragma once
#include <QHash>
#include <QJsonObject>
#include <QJsonValue>
#include <QObject>
#include <QProcess>
#include <QTimer>
#include <functional>
class Backend final : public QObject {
    Q_OBJECT
  public:
    using Reply = std::function<void(const QJsonValue &, const QJsonObject &)>;
    explicit Backend(QObject *parent = nullptr);
    void start(const QString &executable,
               const QProcessEnvironment &environment = QProcessEnvironment::systemEnvironment());
    quint64 request(const QString &method, const QJsonObject &params = {}, Reply reply = {});
    void shutdown();
    static bool decode(const QByteArray &line, QJsonObject *result);
  signals:
    void ready();
    void event(const QString &name, const QJsonValue &data);
    void disconnected(const QString &reason);

  private:
    struct Pending {
        Reply reply;
        qint64 created;
    };
    QProcess process;
    QByteArray buffer;
    QHash<quint64, Pending> pending;
    quint64 nextId = 1;
    QTimer timer;
    void receive();
    void fail(const QString &message);
};
