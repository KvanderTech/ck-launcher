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
    static QVector3D bendVertex(QVector3D point, float pivot, float bend, float axis,
                                bool upper = false);

  private:
    struct Frame {
        double tick, value;
        QString easing;
    };
    QMap<QString, QMap<QString, QVector<Frame>>> tracks;
    double endTick = 0;
    bool easingBefore = true;
    double channel(const QString &part, const QString &axis, double tick, double initial) const;
};
