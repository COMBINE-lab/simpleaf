# image: COMBINE-lab/dockeraf
FROM condaforge/mambaforge:4.14.0-0
MAINTAINER salmon.maintainer@gmail.com

RUN mamba install -c anaconda git
# should pull in salmon, alevin-fry, and pyroe
RUN mamba install -c bioconda simpleaf
RUN mamba install -c conda-forge cxx-compiler
RUN mamba install -c conda-forge r-base r-essentials
RUN mamba install -c bioconda bioconductor-fishpond 
RUN mamba install -c conda-forge time

RUN mkdir /afhome
RUN touch /root/.bashrc \
       && echo "export ALEVIN_FRY_HOME=/afhome" >> /root/.bashrc \
       && echo "export TIME_BIN=/opt/conda/bin/time" >> /root/.bashrc

ENV ALEVIN_FRY_HOME=/afhome
ENV TIME_BIN=/opt/conda/bin/time

RUN simpleaf set-paths

RUN mkdir /afhome/plist
RUN wget -O /afhome/plist/10x_v2_permit.txt https://umd.box.com/shared/static/jbs2wszgbj7k4ic2hass9ts6nhqkwq1p
RUN wget -O /afhome/plist/10x_v3_permit.txt https://umd.box.com/shared/static/eo0qlkfqf2v24ws6dfnxty6gqk1otf2h

RUN touch /root/.Rprofile 

RUN echo 'local({r <- getOption("repos") \n\
      r["CRAN"] <- "http://cran.r-project.org" \n\
      options(repos=r) \n\
      })' >> /root/.Rprofile
