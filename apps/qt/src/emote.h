#pragma once
#include <QJsonObject>
#include <QMap>
#include <QMatrix4x4>
#include <QVector>

struct EmotePart {
    QVector3D position, rotation, scale{1, 1, 1};
    float bend = 0, axis = 0;
};
class EmoteClip {
  public:
    EmoteClip() = default;
    explicit EmoteClip(const QJsonObject &source);
    static EmoteClip load(const QString &resource);
    EmotePart sample(const QString &part, double tick, const QVector3D &bind = {}) const;
    double duration() const {
        return endTick / 20.0;
    }
    bool valid() const {
        return !tracks.isEmpty() && endTick > 0;
    }
    static QMatrix4x4 modelTransform(const EmotePart &part, float weight = 1);
    static QMatrix4x4 bodyTransform(const EmotePart &part, float weight = 1);
    static QMatrix4x4 bendTransform(const QVector3D &center, float bend, float axis,
                                    bool upper = false);
    static QVector3D bendVertex(QVector3D point, const QVector3D &center, float halfLength,
                                float bend, float axis, bool upper = false);

  private:
    struct Frame {
        double tick, value;
        QString easing;
    };
    QMap<QString, QMap<QString, QVector<Frame>>> tracks;
    double beginTick = 0, endTick = 0, stopTick = 0, returnTick = 0;
    bool easingBefore = false, loop = false;
    double channel(const QString &part, const QString &axis, double tick, double initial,
                   bool loopStarted) const;
};
