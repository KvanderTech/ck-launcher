#pragma once
#include <QtWidgets>
class SkinView final : public QWidget {
  public:
    explicit SkinView(bool compact = false, QWidget *parent = nullptr);
    void setSkin(const QImage &image, bool slim = false);
    void setCape(const QImage &image);
    void setAnimated(bool enabled);
    void setPoseTime(double seconds);

  protected:
    void paintEvent(QPaintEvent *) override;
    void mousePressEvent(QMouseEvent *) override;
    void mouseMoveEvent(QMouseEvent *) override;
    void mouseReleaseEvent(QMouseEvent *) override;

  private:
    QImage texture, cape;
    bool compact, slim = false, animated = true;
    QTimer timer;
    QElapsedTimer clock;
    QPoint drag;
    double yaw = -20, fixedTime = -1;
};
